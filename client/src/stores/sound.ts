/**
 * Sound Settings Store
 *
 * Manages notification sound preferences with localStorage persistence.
 */

import { createSignal } from "solid-js";

// ============================================================================
// Types
// ============================================================================

export type SoundOption = "default" | "subtle" | "ping" | "chime" | "bell";
export type NotificationLevel = "all" | "mentions" | "none";

export interface SoundSettings {
  /** Master on/off for notification sounds */
  enabled: boolean;
  /** Volume level 0-100 */
  volume: number;
  /** Selected notification sound */
  selectedSound: SoundOption;
}

export interface ChannelNotificationSettings {
  [channelId: string]: NotificationLevel;
}

// ============================================================================
// Storage Keys
// ============================================================================

const SOUND_SETTINGS_KEY = "canis:sound:settings";
const CHANNEL_SETTINGS_KEY = "canis:sound:channels";

// ============================================================================
// Defaults
// ============================================================================

const defaultSoundSettings: SoundSettings = {
  enabled: true,
  volume: 80,
  selectedSound: "default",
};

// ============================================================================
// Load Functions
// ============================================================================

function loadSoundSettings(): SoundSettings {
  if (typeof localStorage === "undefined") return defaultSoundSettings;
  const stored = localStorage.getItem(SOUND_SETTINGS_KEY);
  if (!stored) return defaultSoundSettings;
  try {
    return { ...defaultSoundSettings, ...JSON.parse(stored) };
  } catch {
    return defaultSoundSettings;
  }
}

function loadChannelSettings(): ChannelNotificationSettings {
  if (typeof localStorage === "undefined") return {};
  const stored = localStorage.getItem(CHANNEL_SETTINGS_KEY);
  if (!stored) return {};
  try {
    return JSON.parse(stored);
  } catch {
    return {};
  }
}

// ============================================================================
// Signals
// ============================================================================

const [soundSettings, setSoundSettings] = createSignal<SoundSettings>(
  loadSoundSettings()
);

const [channelNotificationSettings, setChannelNotificationSettings] =
  createSignal<ChannelNotificationSettings>(loadChannelSettings());

// ============================================================================
// Sound Settings Functions
// ============================================================================

export function getSoundEnabled(): boolean {
  return soundSettings().enabled;
}

export function setSoundEnabled(enabled: boolean): void {
  const updated = { ...soundSettings(), enabled };
  setSoundSettings(updated);
  localStorage.setItem(SOUND_SETTINGS_KEY, JSON.stringify(updated));
}

export function getSoundVolume(): number {
  return soundSettings().volume;
}

export function setSoundVolume(volume: number): void {
  const clamped = Math.max(0, Math.min(100, volume));
  const updated = { ...soundSettings(), volume: clamped };
  setSoundSettings(updated);
  localStorage.setItem(SOUND_SETTINGS_KEY, JSON.stringify(updated));
}

export function getSelectedSound(): SoundOption {
  return soundSettings().selectedSound;
}

export function setSelectedSound(sound: SoundOption): void {
  const updated = { ...soundSettings(), selectedSound: sound };
  setSoundSettings(updated);
  localStorage.setItem(SOUND_SETTINGS_KEY, JSON.stringify(updated));
}

// ============================================================================
// Channel Notification Functions
// ============================================================================

/**
 * Get notification level for a channel.
 * Default is "mentions" for channels, "all" for DMs.
 */
export function getChannelNotificationLevel(
  channelId: string,
  isDm: boolean = false
): NotificationLevel {
  const settings = channelNotificationSettings();
  return settings[channelId] ?? (isDm ? "all" : "mentions");
}

export function setChannelNotificationLevel(
  channelId: string,
  level: NotificationLevel
): void {
  const updated = { ...channelNotificationSettings(), [channelId]: level };
  setChannelNotificationSettings(updated);
  localStorage.setItem(CHANNEL_SETTINGS_KEY, JSON.stringify(updated));
}

/**
 * Check if a channel is muted (notification level = "none").
 */
export function isChannelMuted(channelId: string): boolean {
  return getChannelNotificationLevel(channelId) === "none";
}

// ============================================================================
// Exports
// ============================================================================

export { soundSettings, channelNotificationSettings };
