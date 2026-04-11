/**
 * User settings, preferences, and UI state.
 */

import type { AppSettings, UiState } from "../types";
import { isTauri } from "./common";

export async function getSettings(): Promise<AppSettings> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_settings");
  }

  // Browser mode - return defaults
  return {
    audio: {
      input_device: null,
      output_device: null,
      input_volume: 100,
      output_volume: 100,
      noise_suppression: true,
      echo_cancellation: true,
    },
    voice: {
      push_to_talk: false,
      push_to_talk_key: null,
      push_to_talk_release_delay: 200,
      push_to_mute: false,
      push_to_mute_key: null,
      push_to_mute_release_delay: 200,
      voice_activity_detection: true,
      vad_threshold: 0.5,
    },
    theme: "dark",
    notifications_enabled: true,
  };
}

export async function updateSettings(settings: AppSettings): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_settings", { settings });
  }
  // Browser mode - no-op
}

export async function getUiState(): Promise<UiState> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_ui_state");
  }
  return { category_collapse: {} };
}

export async function updateCategoryCollapse(
  categoryId: string,
  collapsed: boolean,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_category_collapse", { categoryId, collapsed });
  }
}
