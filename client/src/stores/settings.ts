import { createSignal, createResource } from "solid-js";
import { getSettings, updateSettings } from "@/lib/tauri";
import type { AppSettings } from "@/lib/types";

// Note: updateSettings only works in Tauri natively.
// Fallback defaults are provided by getSettings().

const [settingsTrigger, _setSettingsTrigger] = createSignal(0);

const fetchSettings = async () => {
  return await getSettings();
};

const [settingsResource, { mutate }] = createResource(
  settingsTrigger,
  fetchSettings
);

export const appSettings = () => settingsResource();
export const isSettingsLoading = () => settingsResource.loading;
export const settingsError = () => settingsResource.error;

export async function setAppSetting<K extends keyof AppSettings>(
  key: K,
  value: AppSettings[K]
) {
  const current = appSettings();
  if (!current) return;

  const next = { ...current, [key]: value };
  mutate(next); // Optimistic update
  try {
    await updateSettings(next);
  } catch (err) {
    console.error("Failed to update settings:", err);
    // Revert on failure
    mutate(current);
  }
}

export async function updateAudioSetting<K extends keyof AppSettings["audio"]>(
  key: K,
  value: AppSettings["audio"][K]
) {
  const current = appSettings();
  if (!current) return;

  const nextAudio = { ...current.audio, [key]: value };
  await setAppSetting("audio", nextAudio);
}

export async function updateVoiceSetting<K extends keyof AppSettings["voice"]>(
  key: K,
  value: AppSettings["voice"][K]
) {
  const current = appSettings();
  if (!current) return;

  const nextVoice = { ...current.voice, [key]: value };
  await setAppSetting("voice", nextVoice);

  const { updatePttFromSettings, updateVadFromSettings } = await import("@/stores/voice");

  // Re-sync only the subsystem whose keys changed
  const pttKeys: (keyof AppSettings["voice"])[] = [
    "push_to_talk", "push_to_talk_key", "push_to_talk_release_delay",
    "push_to_mute", "push_to_mute_key", "push_to_mute_release_delay",
  ];
  const vadKeys: (keyof AppSettings["voice"])[] = [
    "voice_activity_detection", "vad_threshold",
  ];

  if (pttKeys.includes(key)) {
    await updatePttFromSettings();
  }
  if (vadKeys.includes(key)) {
    updateVadFromSettings();
  }
}
